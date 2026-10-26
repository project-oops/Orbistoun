# D994 - Compiled pipelines are kept one checked entry a pipeline

**Status:** decided
**Date:** 2026-10-26
**known_by:** measured

Amends D744. A title's compiled pipelines are still kept across runs in the titles library, but no
longer as one Vulkan pipeline cache. Each pipeline is built through a pipeline cache of its own,
and that cache's data is kept as one entry under `host-pipelines/`
(`pipeline_cache`).

**Where an entry lives.** A directory is named by the device and driver that made the data:
`<vendor>-<device>-<driver version>-<pipeline-cache UUID>`. In it, an entry file is named by the
pipeline's key:

- a content hash of its SPIR-V;
- a hash of its state. For a guest compute pipeline this is its layout: the images, the buffer
  set, the global share and the push block. For a draw it is the shape key the backend keys its
  pipelines by. A draw with no shape key is compiled and not kept.

**What is checked before the driver sees a byte.** An entry's envelope holds a magic, a format,
the four identity fields, the key, the data's length and a hash of the data. An entry is handed to
`vkCreatePipelineCache` only when every field matches this device, driver and pipeline, the file
is exactly as long as the envelope says, and the data hashes as it says. The data must also begin
with a version-one pipeline-cache header naming this vendor, device and UUID.

**Fallbacks.** Each failure compiles the pipeline afresh:

- An entry that is missing, short, long, corrupt, foreign or another pipeline's is dropped, said
  in the log, and compiled afresh.
- A cache the driver will not make from the entry, or a pipeline it will not build through it, is
  compiled afresh from an empty cache.
- Whether the driver found the pipeline is read from the cache's data size. Unchanged means found;
  grown means the driver compiled it, and the entry is written again.

**A driver that stops while loading.** Loading one title's whole driver cache, hundreds of
megabytes, has been reported to stop a driver on this title. An entry holds one pipeline's data.
While the driver loads it, a marker file stands beside it. The next run drops any entry whose marker
is left from the last, so one bad entry costs one run and one compile, not every boot after.

**Opening the store also drops:**

- another driver's directory, so a driver change compiles afresh;
- partial writes a stopped run left;
- D744's single `host-pipelines.bin`, unread.

Entries are written through a temporary file and a rename.

**The size bound.** The store is kept within 4 GiB, at its opening and again after every 1 GiB a
run writes. Entries are dropped in the order of their stamps, oldest first. Each entry a run
finds or writes is stamped just before that run opened the store, a millisecond earlier for each
one after. So every earlier run's entries go before this run's, and among this run's the last
used go first. A boot's pipelines, the first a run uses, are kept longest.

**Measured** on Grand Theft Auto V, booting to its first in-world ray-traversal dispatch
(`0x74005b7e8600`) under `--limit 540`, the two runs back to back on one build:

| boot | to in-world | pipelines built by then, and their time | whole run |
|---|---|---|---|
| cold, the store empty | 194 s | 750 in 60.1 s | 1,163 in 89.0 s; 2.5 GB of entries written |
| warm, second boot | 126 s | 550 in 5.8 s, 314 found kept | 1,313 in 35.0 s, 599 found; 1.3 GB read, 1.6 GB written |

At the same 750 pipelines, the warm boot had spent 23.8 s against the cold one's 60.1 s.

- **Size.** An entry is one pipeline's compiled code. The smallest compute pipeline's is 6 KB; a
  translated draw's is 2.1-3.0 MB. So a long run writes gigabytes, which the size bound keeps
  within 4 GiB.
- **Misses.** About 700 of the warm boot's pipelines were not in the store. They are draws whose
  shape key the cold boot never formed. One pair of shaders alone took 182 shapes over the two
  runs, its states differing with what each run's frames drew. None was refused.
- **D744's cache, as it was.** It was never measured warm: an earlier run of another session,
  already on this store, dropped its 618 MB file. Starting empty, it reached in-world in 160 s,
  having built 550 pipelines in 29.9 s, on an earlier tree.

The compile watch counts every pipeline it builds (`compile_watch::tally`). It writes a line every
fifty pipelines and once at the run's end. Each line gives how many were built, their time
together, the longest, and how many were found kept.

**Tests.**

- `pipeline_cache`'s unit tests: an entry opens whole only for its own device, driver and key;
  each damage and each foreign field is refused; opening drops a stopped load, another driver's
  entries and partial writes.
- `a_kept_pipeline_is_found_and_a_damaged_or_foreign_entry_compiles_afresh`, on the device: the
  first build compiles and keeps, the second is found. An entry with a byte changed, and one
  enveloped for another driver version, each compile afresh and are written whole again.
- `kept_pipelines.rs`: a guest dispatch's pipeline is kept under this driver's directory, and
  D744's file is dropped.

**Rejected:**

- Keeping one cache whole (D744). Its 618 MB file was read at every boot and handed to the driver
  as it lay. Its header was checked, but nothing showed the rest whole. It was written again, whole,
  every time it grew.
- `VK_KHR_pipeline_binary`, which keys binaries by the driver's own key. The bindings this tree
  uses (ash 0.38, Vulkan 1.3.281) predate it. An entry's own cache gives the same isolation
  through the core API.
- `VK_EXT_pipeline_creation_cache_control`'s fail-if-compile-required flag, to ask whether an entry
  holds the pipeline: the cache's size answers the same after the build, and a miss compiles
  either way.
